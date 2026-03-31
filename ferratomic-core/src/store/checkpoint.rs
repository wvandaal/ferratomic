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

    /// INV-FERR-013: in-memory checkpoint byte round-trip preserves store identity.
    ///
    /// bd-rdvs: Verifies `Store::from_checkpoint_bytes(&store.to_checkpoint_bytes()?)` produces
    /// a store with identical datom set, epoch, schema length, and valid index bijection.
    #[test]
    fn test_inv_ferr_013_store_bytes_roundtrip() {
        use crate::writer::Transaction;

        // -- Empty genesis store round-trips --
        let genesis = Store::genesis();
        let bytes = genesis
            .to_checkpoint_bytes()
            .expect("INV-FERR-013: genesis serialization must succeed");
        let loaded = Store::from_checkpoint_bytes(&bytes)
            .expect("INV-FERR-013: genesis deserialization must succeed");

        assert_eq!(
            *loaded.datom_set(),
            *genesis.datom_set(),
            "INV-FERR-013: genesis datom set must survive bytes round-trip"
        );
        assert_eq!(
            loaded.epoch(),
            genesis.epoch(),
            "INV-FERR-013: genesis epoch must survive bytes round-trip"
        );
        assert_eq!(
            loaded.schema().len(),
            genesis.schema().len(),
            "INV-FERR-013: genesis schema must survive bytes round-trip"
        );

        // -- Store with datoms round-trips --
        let mut store = Store::genesis();
        let tx = Transaction::new(store.genesis_agent())
            .assert_datom(
                EntityId::from_content(b"entity-bytes-1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("bytes round-trip test")),
            )
            .commit_unchecked();
        store.transact(tx).expect("transact ok");

        let bytes = store
            .to_checkpoint_bytes()
            .expect("INV-FERR-013: serialization must succeed");
        let loaded = Store::from_checkpoint_bytes(&bytes)
            .expect("INV-FERR-013: deserialization must succeed");

        assert_eq!(
            *loaded.datom_set(),
            *store.datom_set(),
            "INV-FERR-013: datom set must be identical after bytes round-trip"
        );
        assert_eq!(
            loaded.epoch(),
            store.epoch(),
            "INV-FERR-013: epoch must be preserved after bytes round-trip"
        );
        assert_eq!(
            loaded.schema().len(),
            store.schema().len(),
            "INV-FERR-013: schema must be preserved after bytes round-trip"
        );
        assert!(
            loaded.indexes().verify_bijection(),
            "INV-FERR-005: all indexes must have same cardinality after bytes round-trip"
        );
        assert_eq!(
            loaded.indexes().len(),
            loaded.len(),
            "INV-FERR-005: index len must match primary after bytes round-trip"
        );

        // -- Bytes match what write_checkpoint would produce --
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
