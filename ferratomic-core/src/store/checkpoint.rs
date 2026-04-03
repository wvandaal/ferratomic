//! Checkpoint byte serialization convenience methods for [`Store`].
//!
//! These are thin wrappers over [`crate::checkpoint::serialize_checkpoint_bytes`]
//! and [`crate::checkpoint::deserialize_checkpoint_bytes`], providing the
//! ergonomic `store.to_checkpoint_bytes()` / `Store::from_checkpoint_bytes()`
//! API for in-memory round-trip.
//!
//! INV-FERR-013: `Store::from_checkpoint_bytes(&store.to_checkpoint_bytes()?) == store`.

use ferratom::FerraError;

use super::Store;

impl Store {
    /// Serialize the store to checkpoint bytes.
    ///
    /// INV-FERR-013: `Store::from_checkpoint_bytes(&store.to_checkpoint_bytes()?) == store`.
    /// The byte format is identical to what [`write_checkpoint`](crate::checkpoint::write_checkpoint)
    /// produces: magic, version, epoch, JSON payload, BLAKE3 hash.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::CheckpointWrite` if serialization fails
    /// (e.g., payload exceeds `u32::MAX` bytes).
    pub fn to_checkpoint_bytes(&self) -> Result<Vec<u8>, FerraError> {
        crate::checkpoint::serialize_checkpoint_bytes(self)
    }

    /// Reconstruct a store from checkpoint bytes.
    ///
    /// INV-FERR-013: round-trip identity with [`to_checkpoint_bytes`](Self::to_checkpoint_bytes).
    /// INV-FERR-005: indexes are rebuilt from the deserialized datom set.
    /// The byte format must match what `to_checkpoint_bytes` or
    /// [`write_checkpoint`](crate::checkpoint::write_checkpoint) produces.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
    /// truncation, wrong magic, or deserialization failure.
    pub fn from_checkpoint_bytes(data: &[u8]) -> Result<Self, FerraError> {
        crate::checkpoint::deserialize_checkpoint_bytes(data)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, EntityId, Value};

    use super::*;

    /// Helper: assert that a loaded store matches the original after bytes round-trip.
    fn assert_bytes_roundtrip(original: &Store, label: &str) -> Vec<u8> {
        let bytes = original
            .to_checkpoint_bytes()
            .unwrap_or_else(|_| panic!("INV-FERR-013: {label} serialization must succeed"));
        let loaded = Store::from_checkpoint_bytes(&bytes)
            .unwrap_or_else(|_| panic!("INV-FERR-013: {label} deserialization must succeed"));

        assert_eq!(
            loaded.datom_set(),
            original.datom_set(),
            "INV-FERR-013: {label} datom set must survive bytes round-trip"
        );
        assert_eq!(
            loaded.epoch(),
            original.epoch(),
            "INV-FERR-013: {label} epoch must survive bytes round-trip"
        );
        assert_eq!(
            loaded.schema().len(),
            original.schema().len(),
            "INV-FERR-013: {label} schema must survive bytes round-trip"
        );
        bytes
    }

    /// INV-FERR-013: genesis store bytes round-trip preserves identity.
    #[test]
    fn test_inv_ferr_013_genesis_bytes_roundtrip() {
        let genesis = Store::genesis();
        assert_bytes_roundtrip(&genesis, "genesis");
    }

    /// INV-FERR-013: store with datoms bytes round-trip preserves identity,
    /// indexes, and matches file-based checkpoint output.
    #[test]
    fn test_inv_ferr_013_store_bytes_roundtrip() {
        use crate::writer::Transaction;

        let mut store = Store::genesis();
        let tx = Transaction::new(store.genesis_agent())
            .assert_datom(
                EntityId::from_content(b"entity-bytes-1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("bytes round-trip test")),
            )
            .commit_unchecked();
        store.transact_test(tx).expect("transact ok");

        let bytes = assert_bytes_roundtrip(&store, "datoms");

        // bd-h2fz: from_checkpoint builds Positional repr. Promote to
        // OrdMap to verify index bijection via the Indexes API.
        let mut loaded = Store::from_checkpoint_bytes(&bytes).expect("reload ok");
        loaded.promote();
        assert!(
            loaded.indexes().unwrap().verify_bijection(),
            "INV-FERR-005: all indexes must have same cardinality after bytes round-trip"
        );
        assert_eq!(
            loaded.indexes().unwrap().len(),
            loaded.len(),
            "INV-FERR-005: index len must match primary after bytes round-trip"
        );

        // Bytes match what write_checkpoint would produce
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let path = dir.path().join("compare.chkp");
        crate::checkpoint::write_checkpoint(&store, &path).expect("write ok");
        let file_bytes = std::fs::read(&path).expect("read ok");
        assert_eq!(
            bytes, file_bytes,
            "INV-FERR-013: to_checkpoint_bytes must produce identical bytes as write_checkpoint"
        );
    }
}
