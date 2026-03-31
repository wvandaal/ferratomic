//! Checkpoint: serialize Store to a durable file with BLAKE3 integrity.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` — round-trip identity.
//! The datom set, indexes, schema, and epoch are preserved exactly
//! through serialization and deserialization. No datom is lost, added,
//! or reordered.
//!
//! # File Format
//!
//! ```text
//! +------------------+
//! | Magic    (4B)    | 0x43484B50 ("CHKP")
//! +------------------+
//! | Version  (2B)    | 0x0001 (little-endian)
//! +------------------+
//! | Epoch    (8B)    | u64 little-endian
//! +------------------+
//! | Length   (4B)    | u32 byte count of JSON payload
//! +------------------+
//! | Payload  (N)     | JSON: { schema, genesis_agent, datoms }
//! +------------------+
//! | BLAKE3   (32B)   | Hash of all preceding bytes
//! +------------------+
//! ```
//!
//! The payload uses JSON serialization for consistency with the WAL
//! (Phase 4a). Phase 4b may switch to bincode or compressed format
//! for the INV-FERR-028 cold-start target at 100M datoms.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufWriter, Write as IoWrite},
    path::Path,
};

use ferratom::{AgentId, AttributeDef, Datom, FerraError};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// Checkpoint file magic bytes: ASCII "CHKP".
const CHECKPOINT_MAGIC: [u8; 4] = *b"CHKP";

/// Checkpoint format version. Little-endian u16.
const CHECKPOINT_VERSION: u16 = 1;

/// Fixed header size: magic(4) + version(2) + epoch(8) + length(4) = 18 bytes.
const HEADER_SIZE: usize = 18;

/// BLAKE3 hash size: 32 bytes.
const HASH_SIZE: usize = 32;

/// Minimum checkpoint file size: header + hash (empty payload).
const MIN_FILE_SIZE: usize = HEADER_SIZE + HASH_SIZE;

/// JSON-serializable checkpoint payload.
///
/// Schema attributes are sorted by name for deterministic output.
/// Datoms are in `OrdSet` iteration order (`Datom`'s `Ord` impl = EAVT).
#[derive(Serialize, Deserialize)]
struct CheckpointPayload {
    /// Schema attributes as sorted (name, definition) pairs.
    schema: Vec<(String, AttributeDef)>,
    /// The genesis agent identity for Store reconstruction.
    genesis_agent: AgentId,
    /// All datoms in deterministic EAVT order.
    datoms: Vec<Datom>,
}

/// Serialize a store to checkpoint bytes (in-memory).
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// schema, genesis agent, all datoms) in the checkpoint wire format.
/// A trailing BLAKE3 hash covers all preceding bytes for tamper detection.
/// `deserialize_checkpoint_bytes` can reconstruct the store exactly.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if the JSON payload exceeds
/// `u32::MAX` bytes or serialization fails.
pub fn serialize_checkpoint_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let epoch = store.epoch();

    // Build deterministic payload: schema sorted by attribute name,
    // datoms in OrdSet iteration order (EAVT by Datom::Ord).
    let schema: Vec<(String, AttributeDef)> = {
        let mut sorted: BTreeMap<String, AttributeDef> = BTreeMap::new();
        for (attr, def) in store.schema().iter() {
            sorted.insert(attr.as_str().to_owned(), def.clone());
        }
        sorted.into_iter().collect()
    };

    let payload = CheckpointPayload {
        schema,
        genesis_agent: store.genesis_agent(),
        datoms: store.datoms().cloned().collect(),
    };

    let payload_bytes =
        serde_json::to_vec(&payload).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    let payload_len = u32::try_from(payload_bytes.len()).map_err(|_| {
        FerraError::CheckpointWrite(format!(
            "payload too large: {} bytes exceeds u32::MAX",
            payload_bytes.len()
        ))
    })?;

    // Build the content: header + payload + hash.
    let total_size = HEADER_SIZE + payload_bytes.len() + HASH_SIZE;
    let mut buf = Vec::with_capacity(total_size);

    // Header
    buf.extend_from_slice(&CHECKPOINT_MAGIC);
    buf.extend_from_slice(&CHECKPOINT_VERSION.to_le_bytes());
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(&payload_len.to_le_bytes());

    // Payload
    buf.extend_from_slice(&payload_bytes);

    // BLAKE3 hash of [magic..payload]
    let hash = blake3::hash(&buf);
    buf.extend_from_slice(hash.as_bytes());

    Ok(buf)
}

/// Deserialize a store from checkpoint bytes (in-memory).
///
/// INV-FERR-013: Verifies the BLAKE3 checksum before reconstructing
/// the store. Returns an error if the data is truncated, the magic
/// is wrong, or the checksum fails. Indexes are rebuilt from the
/// deserialized datom set (INV-FERR-005 by construction).
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// format errors, or deserialization failure.
pub fn deserialize_checkpoint_bytes(data: &[u8]) -> Result<Store, FerraError> {
    if data.len() < MIN_FILE_SIZE {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("at least {MIN_FILE_SIZE} bytes"),
            actual: format!("{} bytes", data.len()),
        });
    }

    // Split into [header+payload] and [hash].
    let (content, hash_bytes) = data.split_at(data.len() - HASH_SIZE);
    verify_checksum(content, hash_bytes)?;

    // Parse header and extract payload.
    let (epoch, payload_bytes) = parse_header(content)?;

    // Deserialize and reconstruct.
    let payload: CheckpointPayload =
        serde_json::from_slice(payload_bytes).map_err(|e| FerraError::CheckpointCorrupted {
            expected: "valid JSON payload".to_string(),
            actual: e.to_string(),
        })?;

    Ok(Store::from_checkpoint(
        epoch,
        payload.genesis_agent,
        payload.schema,
        payload.datoms,
    ))
}

/// Serialize a store to a checkpoint file.
///
/// INV-FERR-013: The checkpoint contains the full store state (epoch,
/// schema, genesis agent, all datoms) in a format that `load_checkpoint`
/// can reconstruct exactly. A trailing BLAKE3 hash covers all preceding
/// bytes for tamper detection.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if file creation, serialization,
/// or fsync fails.
pub fn write_checkpoint(store: &Store, path: &Path) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(store)?;

    // Write atomically: write to file, then fsync.
    let file = File::create(path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&buf)
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    writer
        .flush()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    Ok(())
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
    let data = std::fs::read(path).map_err(|e| FerraError::Io(e.to_string()))?;

    deserialize_checkpoint_bytes(&data)
}

/// Load a checkpoint from an arbitrary reader (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint loading for `StorageBackend` implementations.
///
/// # Errors
///
/// Returns `FerraError::Io` on read failure or `FerraError::CheckpointCorrupted`
/// on checksum/format errors.
pub fn load_checkpoint_from_reader<R: std::io::Read>(reader: &mut R) -> Result<Store, FerraError> {
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| FerraError::Io(e.to_string()))?;
    deserialize_checkpoint_bytes(&data)
}

/// Write a checkpoint to an arbitrary writer (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint writing for `StorageBackend` implementations.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization or the write fails.
pub fn write_checkpoint_to_writer<W: std::io::Write>(
    store: &Store,
    writer: &mut W,
) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(store)?;
    writer
        .write_all(&buf)
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    writer
        .flush()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    Ok(())
}

/// Verify the BLAKE3 checksum of the content against the stored hash.
fn verify_checksum(content: &[u8], hash_bytes: &[u8]) -> Result<(), FerraError> {
    let computed = blake3::hash(content);
    if computed.as_bytes() != hash_bytes {
        return Err(FerraError::CheckpointCorrupted {
            expected: hex_encode(computed.as_bytes()),
            actual: hex_encode(hash_bytes),
        });
    }
    Ok(())
}

/// Parse the fixed header and validate magic/version.
/// Returns `(epoch, payload_slice)`.
fn parse_header(content: &[u8]) -> Result<(u64, &[u8]), FerraError> {
    if content.len() < HEADER_SIZE {
        return Err(FerraError::CheckpointCorrupted {
            expected: "valid header".to_string(),
            actual: "truncated header".to_string(),
        });
    }

    let magic: [u8; 4] = content[0..4]
        .try_into()
        .map_err(|_| FerraError::CheckpointCorrupted {
            expected: "CHKP magic".to_string(),
            actual: "invalid magic".to_string(),
        })?;
    if magic != CHECKPOINT_MAGIC {
        return Err(FerraError::CheckpointCorrupted {
            expected: "CHKP".to_string(),
            actual: String::from_utf8_lossy(&magic).to_string(),
        });
    }

    let version = u16::from_le_bytes(content[4..6].try_into().map_err(|_| {
        FerraError::CheckpointCorrupted {
            expected: "2-byte version".to_string(),
            actual: "truncated".to_string(),
        }
    })?);
    if version != CHECKPOINT_VERSION {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("version {CHECKPOINT_VERSION}"),
            actual: format!("version {version}"),
        });
    }

    let epoch = u64::from_le_bytes(content[6..14].try_into().map_err(|_| {
        FerraError::CheckpointCorrupted {
            expected: "8-byte epoch".to_string(),
            actual: "truncated".to_string(),
        }
    })?);

    let payload_len = u32::from_le_bytes(content[14..18].try_into().map_err(|_| {
        FerraError::CheckpointCorrupted {
            expected: "4-byte payload length".to_string(),
            actual: "truncated".to_string(),
        }
    })?) as usize;

    let available = content.len() - HEADER_SIZE;
    if available < payload_len {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("{payload_len} bytes payload"),
            actual: format!("{available} bytes available"),
        });
    }

    Ok((epoch, &content[HEADER_SIZE..HEADER_SIZE + payload_len]))
}

/// Encode bytes as hex string for error messages.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, EntityId, Value};

    use super::*;
    use crate::writer::Transaction;

    #[test]
    fn test_inv_ferr_013_roundtrip_empty() {
        let store = Store::genesis();
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("checkpoint.chkp");

        write_checkpoint(&store, &path).unwrap();
        let loaded = load_checkpoint(&path).unwrap();

        assert_eq!(loaded.epoch(), store.epoch());
        assert_eq!(loaded.len(), store.len());
        assert_eq!(*loaded.datom_set(), *store.datom_set());
        assert_eq!(loaded.schema().len(), store.schema().len());
    }

    #[test]
    fn test_inv_ferr_013_roundtrip_with_datoms() {
        let mut store = Store::genesis();

        // Transact some datoms.
        let tx = Transaction::new(store.genesis_agent())
            .assert_datom(
                EntityId::from_content(b"entity-1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("hello world")),
            )
            .commit_unchecked();
        store.transact(tx).unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("checkpoint.chkp");

        write_checkpoint(&store, &path).unwrap();
        let loaded = load_checkpoint(&path).unwrap();

        // INV-FERR-013: datom set identity.
        assert_eq!(
            *loaded.datom_set(),
            *store.datom_set(),
            "INV-FERR-013: datom set must be identical after roundtrip"
        );
        // Epoch identity.
        assert_eq!(
            loaded.epoch(),
            store.epoch(),
            "INV-FERR-013: epoch must be preserved"
        );
        // Schema identity.
        assert_eq!(
            loaded.schema().len(),
            store.schema().len(),
            "INV-FERR-013: schema must be preserved"
        );
        // Index bijection on loaded store (INV-FERR-005).
        assert!(
            loaded.indexes().verify_bijection(),
            "INV-FERR-005: all indexes must have same cardinality after load"
        );
        assert_eq!(
            loaded.indexes().len(),
            loaded.len(),
            "INV-FERR-005: index len must match primary after load"
        );
    }

    #[test]
    fn test_inv_ferr_013_corrupted_rejected() {
        let store = Store::genesis();
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("checkpoint.chkp");

        write_checkpoint(&store, &path).unwrap();

        // Corrupt a byte in the middle of the file.
        let mut data = std::fs::read(&path).unwrap();
        let mid = data.len() / 2;
        data[mid] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();

        // Must be rejected.
        let result = load_checkpoint(&path);
        assert!(
            result.is_err(),
            "INV-FERR-013: corrupted checkpoint must be rejected"
        );
    }

    #[test]
    fn test_inv_ferr_013_truncated_rejected() {
        let store = Store::genesis();
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("checkpoint.chkp");

        write_checkpoint(&store, &path).unwrap();

        // Truncate the file.
        let data = std::fs::read(&path).unwrap();
        std::fs::write(&path, &data[..data.len() / 2]).unwrap();

        let result = load_checkpoint(&path);
        assert!(
            result.is_err(),
            "INV-FERR-013: truncated checkpoint must be rejected"
        );
    }

    #[test]
    fn test_inv_ferr_013_wrong_magic_rejected() {
        let store = Store::genesis();
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("checkpoint.chkp");

        write_checkpoint(&store, &path).unwrap();

        // Overwrite magic bytes.
        let mut data = std::fs::read(&path).unwrap();
        data[0..4].copy_from_slice(b"XXXX");
        // Recompute hash so corruption is only in magic.
        let content_len = data.len() - HASH_SIZE;
        let hash = blake3::hash(&data[..content_len]);
        data[content_len..].copy_from_slice(hash.as_bytes());
        std::fs::write(&path, &data).unwrap();

        let result = load_checkpoint(&path);
        assert!(
            result.is_err(),
            "INV-FERR-013: wrong magic must be rejected"
        );
    }

    #[test]
    fn test_inv_ferr_013_deterministic_output() {
        let store = Store::genesis();
        let dir = tempfile::TempDir::new().unwrap();

        let path1 = dir.path().join("a.chkp");
        let path2 = dir.path().join("b.chkp");

        write_checkpoint(&store, &path1).unwrap();
        write_checkpoint(&store, &path2).unwrap();

        let data1 = std::fs::read(&path1).unwrap();
        let data2 = std::fs::read(&path2).unwrap();

        assert_eq!(
            data1, data2,
            "INV-FERR-031: genesis checkpoint must be deterministic"
        );
    }
}
