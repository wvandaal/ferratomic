//! Checkpoint: serialize Store to a durable file with BLAKE3 integrity.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` — round-trip identity.
//! The datom set, indexes, schema, and epoch are preserved exactly
//! through serialization and deserialization. No datom is lost, added,
//! or reordered.
//!
//! This module is a thin facade over `ferratomic_checkpoint`. It adapts
//! the raw-data API (which accepts/returns `CheckpointData`) to the
//! Store-aware API needed by ferratomic-db.
//!
//! ## Format dispatch
//!
//! Deserialization dispatches on the first 4 magic bytes:
//! - `b"CHKP"` — V2 (legacy)
//! - `b"CHK3"` — V3 (pre-sorted, LIVE bitvector persisted)
//!
//! Serialization always produces V3. V2 read support is retained for
//! backward compatibility with existing checkpoint files.

use std::path::Path;

use ferratom::FerraError;

use crate::store::Store;

#[cfg(test)]
mod tests;

// The v3 submodule is kept for backward compatibility but is now a thin
// facade over ferratomic_checkpoint::v3. The old checkpoint/v3.rs file
// still exists in the source tree.
mod v3;

/// Serialize a store to checkpoint bytes (in-memory) using V3 format.
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// schema, genesis node, all datoms, LIVE bitvector) in the V3 checkpoint
/// wire format. A trailing BLAKE3 hash covers all preceding bytes for
/// tamper detection. `deserialize_checkpoint_bytes` can reconstruct the
/// store exactly.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub fn serialize_checkpoint_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let data = ferratomic_store::extract_checkpoint_data(store);
    ferratomic_checkpoint::serialize_checkpoint_bytes(&data)
}

/// Serialize a store to LIVE-first V3 checkpoint bytes (INV-FERR-075).
///
/// LIVE datoms are stored first, historical datoms second. Version field
/// 0x0103 distinguishes from standard V3. Use `deserialize_checkpoint_bytes`
/// (which handles version dispatch) or `deserialize_live_first_partial` for
/// LIVE-only cold start.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub fn serialize_live_first_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let data = ferratomic_store::extract_checkpoint_data(store);
    ferratomic_checkpoint::serialize_live_first_bytes(&data)
}

/// Deserialize a LIVE-first V3 checkpoint into a partial store (INV-FERR-075).
///
/// Returns a `PartialStore` with LIVE-only data. Call `live_store()` for
/// current-state queries or `load_historical()` to merge retained
/// historical datoms into the full store.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on integrity or format errors.
pub fn deserialize_live_first_partial(data: &[u8]) -> Result<PartialStore, FerraError> {
    let partial_data = ferratomic_checkpoint::deserialize_live_first_partial(data)?;

    // Build LIVE-only Store: all bits are true (every datom in
    // the LIVE partition is live by definition).
    let live_bits = bitvec::prelude::BitVec::repeat(true, partial_data.live_datoms.len());
    let store = Store::from_checkpoint_v3(
        partial_data.epoch,
        partial_data.genesis_node,
        partial_data.schema_pairs.clone(),
        partial_data.live_datoms,
        live_bits,
    )?;

    Ok(PartialStore {
        store,
        hist_datoms: partial_data.hist_datoms,
        schema_pairs: partial_data.schema_pairs,
    })
}

/// LIVE-only store with retained historical datoms for lazy merge (INV-FERR-075).
///
/// Created by `deserialize_live_first_partial()`. The `store` field
/// contains only LIVE datoms. Call `load_historical()` to merge with
/// retained historical datoms and produce the full store.
pub struct PartialStore {
    /// Store built from LIVE datoms only (INV-FERR-029).
    store: Store,
    /// Historical datoms (already trusted — BLAKE3 verified at load).
    hist_datoms: Vec<ferratom::Datom>,
    /// Schema pairs for reconstruction.
    schema_pairs: Vec<(String, ferratom::AttributeDef)>,
}

impl PartialStore {
    /// Access the LIVE-only store for current-state queries (INV-FERR-075, INV-FERR-029).
    ///
    /// The returned store contains only LIVE datoms — the latest Assert for each
    /// `(entity, attribute, value)` group. Sufficient for applications that need
    /// only the current state. Call `load_historical()` to merge retained
    /// historical datoms when temporal queries are needed.
    #[must_use]
    pub fn live_store(&self) -> &Store {
        &self.store
    }

    /// Merge LIVE + HISTORICAL datoms into complete Store (INV-FERR-075).
    ///
    /// Five O(n) passes: merge-sort, positional construction, LIVE bitvector,
    /// `live_causal` rebuild, `live_set` derivation. Uses `from_checkpoint_v3`
    /// to avoid redundant O(n log n) re-sort on already-sorted merge output.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the merged datoms violate
    /// INV-FERR-076 preconditions (should not happen with valid checkpoint data).
    pub fn load_historical(self) -> Result<Store, FerraError> {
        let live_datoms: Vec<ferratom::Datom> = self.store.datoms().cloned().collect();
        let merged = crate::positional::merge_sort_dedup(&live_datoms, &self.hist_datoms);
        let live_bits = crate::positional::build_live_bitvector_pub(&merged);
        Store::from_checkpoint_v3(
            self.store.epoch(),
            self.store.genesis_node(),
            self.schema_pairs,
            merged,
            live_bits,
        )
    }
}

/// Deserialize a store from checkpoint bytes (in-memory), dispatching
/// on magic bytes.
///
/// INV-FERR-013: Examines the first 4 bytes to determine the format:
/// - `b"CHKP"` → V2 (legacy)
/// - `b"CHK3"` → V3 (pre-sorted, LIVE bitvector persisted)
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on unknown magic, checksum
/// mismatch, format errors, or deserialization failure.
pub fn deserialize_checkpoint_bytes(data: &[u8]) -> Result<Store, FerraError> {
    let checkpoint_data = ferratomic_checkpoint::deserialize_checkpoint_bytes(data)?;
    ferratomic_store::store_from_checkpoint_data(checkpoint_data)
}

/// Serialize a store to a checkpoint file.
///
/// INV-FERR-013: The checkpoint contains the full store state (epoch,
/// schema, genesis node, all datoms) in a format that `load_checkpoint`
/// can reconstruct exactly. A trailing BLAKE3 hash covers all preceding
/// bytes for tamper detection.
///
/// HI-001: Write is atomic via write-to-temp-then-rename. A crash during
/// write leaves the old checkpoint intact (the temp file is discarded).
/// HI-003: Parent directory is fsynced after rename to ensure the new
/// directory entry is durable on ext4/XFS.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if file creation, serialization,
/// or fsync fails.
pub fn write_checkpoint(store: &Store, path: &Path) -> Result<(), FerraError> {
    let data = ferratomic_store::extract_checkpoint_data(store);
    ferratomic_checkpoint::write_checkpoint(&data, path)
}

/// Write a LIVE-first V3 checkpoint to a file (INV-FERR-075).
///
/// Same atomic write pattern as `write_checkpoint` (HI-001, HI-003).
/// LIVE datoms are stored first for partial cold start.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization, write, or
/// fsync fails.
pub fn write_checkpoint_live_first(store: &Store, path: &Path) -> Result<(), FerraError> {
    let data = ferratomic_store::extract_checkpoint_data(store);
    ferratomic_checkpoint::write_checkpoint_live_first(&data, path)
}

/// Load a store from a checkpoint file.
///
/// INV-FERR-013: Verifies the BLAKE3 checksum before reconstructing
/// the store. Returns an error if the file is truncated, the magic
/// is wrong, or the checksum fails. Indexes are rebuilt from the
/// deserialized datom set (INV-FERR-005 by construction).
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// `FerraError::Io` on read failure, or `FerraError::CheckpointWrite`
/// on deserialization failure.
pub fn load_checkpoint(path: &Path) -> Result<Store, FerraError> {
    let checkpoint_data = ferratomic_checkpoint::load_checkpoint(path)?;
    ferratomic_store::store_from_checkpoint_data(checkpoint_data)
}

/// Load a checkpoint from an arbitrary reader (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint loading for `StorageBackend` implementations.
///
/// # Errors
///
/// Returns `FerraError::Io` on read failure or `FerraError::CheckpointCorrupted`
/// on checksum/format errors.
pub(crate) fn load_checkpoint_from_reader<R: std::io::Read>(
    reader: &mut R,
) -> Result<Store, FerraError> {
    let checkpoint_data = ferratomic_checkpoint::load_checkpoint_from_reader(reader)?;
    ferratomic_store::store_from_checkpoint_data(checkpoint_data)
}

/// Write a checkpoint to an arbitrary writer (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint writing for [`StorageBackend`](crate::storage::StorageBackend)
/// implementations. The checkpoint contains the full store state (epoch,
/// schema, genesis node, all datoms, LIVE bitvector) in V3 format with
/// a trailing BLAKE3 integrity hash.
///
/// INV-FERR-013: `load(checkpoint(S)) = S` -- round-trip identity.
/// INV-FERR-024: substrate agnosticism -- writes through any `std::io::Write`
/// implementor, decoupling the checkpoint protocol from filesystem specifics.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization or the write fails.
pub fn write_checkpoint_to_writer<W: std::io::Write>(
    store: &Store,
    writer: &mut W,
) -> Result<(), FerraError> {
    let data = ferratomic_store::extract_checkpoint_data(store);
    ferratomic_checkpoint::write_checkpoint_to_writer(&data, writer)
}

// Internal helpers `extract_checkpoint_data` and `store_from_checkpoint_data`
// have been moved to `ferratomic_store` (the canonical owner of Store).
// This module delegates to `ferratomic_store::{extract_checkpoint_data, store_from_checkpoint_data}`.
