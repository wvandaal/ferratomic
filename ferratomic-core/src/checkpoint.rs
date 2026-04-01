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
//! | Length   (4B)    | u32 byte count of bincode payload
//! +------------------+
//! | Payload  (N)     | bincode: { schema, genesis_agent, datoms }
//! +------------------+
//! | BLAKE3   (32B)   | Hash of all preceding bytes
//! +------------------+
//! ```
//!
//! CR-005: The payload uses bincode serialization (matching the WAL format)
//! for INV-FERR-028 compliance. At 100M datoms, bincode produces ~20GB
//! (parseable in <5s on `NVMe`). Per ADR-FERR-010, deserialization goes through
//! wire types (`WireCheckpointPayload`) for trust boundary enforcement.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufWriter, Write as IoWrite},
    path::Path,
};

use ferratom::{AgentId, AttributeDef, Datom, FerraError};
use serde::Serialize;

use crate::store::Store;

#[cfg(test)]
mod tests;

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

/// JSON-serializable checkpoint payload (serialization only).
///
/// ADR-FERR-010: Deserialization uses `WireCheckpointPayload` from the
/// `ferratom::wire` module instead. This struct retains `Serialize` only.
/// Schema attributes are sorted by name for deterministic output.
/// Datoms are in `OrdSet` iteration order (`Datom`'s `Ord` impl = EAVT).
#[derive(Serialize)]
struct CheckpointPayload {
    /// Schema attributes as sorted (name, definition) pairs.
    schema: Vec<(String, AttributeDef)>,
    /// The genesis agent identity for Store reconstruction.
    genesis_agent: AgentId,
    /// All datoms in deterministic EAVT order.
    datoms: Vec<Datom>,
}

/// Build and serialize the checkpoint payload (schema + agent + datoms).
///
/// INV-FERR-013: schema is sorted by attribute name for determinism.
/// Datoms are in `OrdSet` iteration order (`Datom::Ord` = EAVT).
/// CR-005: bincode for INV-FERR-028 cold-start compliance.
fn build_payload_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
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

    bincode::serialize(&payload).map_err(|e| FerraError::CheckpointWrite(e.to_string()))
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
/// Returns `FerraError::CheckpointWrite` if the payload exceeds
/// `u32::MAX` bytes or serialization fails.
pub(crate) fn serialize_checkpoint_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let epoch = store.epoch();
    let payload_bytes = build_payload_bytes(store)?;

    let payload_len = u32::try_from(payload_bytes.len()).map_err(|_| {
        FerraError::CheckpointWrite(format!(
            "payload too large: {} bytes exceeds u32::MAX",
            payload_bytes.len()
        ))
    })?;

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
pub(crate) fn deserialize_checkpoint_bytes(data: &[u8]) -> Result<Store, FerraError> {
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

    // CR-005 + ADR-FERR-010: Deserialize as wire checkpoint payload using
    // bincode, then convert through trust boundary. BLAKE3 verified above.
    let wire_payload: ferratom::wire::WireCheckpointPayload =
        bincode::deserialize(payload_bytes).map_err(|e| FerraError::CheckpointCorrupted {
            expected: "valid bincode payload".to_string(),
            actual: e.to_string(),
        })?;

    // ADR-FERR-010: Convert wire datoms to core datoms through trust boundary.
    let datoms: Vec<ferratom::Datom> = wire_payload
        .datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();

    Ok(Store::from_checkpoint(
        epoch,
        wire_payload.genesis_agent,
        wire_payload.schema,
        datoms,
    ))
}

/// Serialize a store to a checkpoint file.
///
/// INV-FERR-013: The checkpoint contains the full store state (epoch,
/// schema, genesis agent, all datoms) in a format that `load_checkpoint`
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
    let buf = serialize_checkpoint_bytes(store)?;

    // HI-001: Atomic write via temp file + rename. A crash between
    // temp creation and rename leaves the original checkpoint intact.
    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(".checkpoint.tmp");

    // Write to temp file and fsync the data.
    {
        let file =
            File::create(&tmp_path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
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
    }

    // Atomic rename (POSIX guarantees atomicity for same-filesystem rename).
    std::fs::rename(&tmp_path, path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    // HI-003: fsync parent directory to ensure the new directory entry
    // is durable. Required on ext4/XFS for metadata durability.
    fsync_parent_dir(parent)?;

    Ok(())
}

/// Fsync a parent directory to ensure directory entry durability (HI-002, HI-003).
///
/// Required on ext4, XFS, and other journaling filesystems where file
/// data may be durable but directory entries are not until the parent
/// directory is fsynced.
fn fsync_parent_dir(dir: &Path) -> Result<(), FerraError> {
    let dir_file = File::open(dir).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    dir_file
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
pub(crate) fn load_checkpoint_from_reader<R: std::io::Read>(
    reader: &mut R,
) -> Result<Store, FerraError> {
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
    ensure_header_len(content)?;
    validate_magic(parse_magic(content)?)?;
    validate_version(parse_version(content)?)?;

    let epoch = parse_epoch(content)?;
    let payload_len = parse_payload_len(content)?;
    let payload = payload_slice(content, payload_len)?;
    Ok((epoch, payload))
}

fn ensure_header_len(content: &[u8]) -> Result<(), FerraError> {
    if content.len() < HEADER_SIZE {
        return Err(FerraError::CheckpointCorrupted {
            expected: "valid header".to_string(),
            actual: "truncated header".to_string(),
        });
    }
    Ok(())
}

fn parse_magic(content: &[u8]) -> Result<[u8; 4], FerraError> {
    content[0..4]
        .try_into()
        .map_err(|_| FerraError::CheckpointCorrupted {
            expected: "CHKP magic".to_string(),
            actual: "invalid magic".to_string(),
        })
}

fn validate_magic(magic: [u8; 4]) -> Result<(), FerraError> {
    if magic != CHECKPOINT_MAGIC {
        return Err(FerraError::CheckpointCorrupted {
            expected: "CHKP".to_string(),
            actual: String::from_utf8_lossy(&magic).to_string(),
        });
    }
    Ok(())
}

fn parse_version(content: &[u8]) -> Result<u16, FerraError> {
    Ok(u16::from_le_bytes(read_header_bytes(
        content,
        4,
        "2-byte version",
    )?))
}

fn validate_version(version: u16) -> Result<(), FerraError> {
    if version != CHECKPOINT_VERSION {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("version {CHECKPOINT_VERSION}"),
            actual: format!("version {version}"),
        });
    }
    Ok(())
}

fn parse_epoch(content: &[u8]) -> Result<u64, FerraError> {
    Ok(u64::from_le_bytes(read_header_bytes(
        content,
        6,
        "8-byte epoch",
    )?))
}

fn parse_payload_len(content: &[u8]) -> Result<usize, FerraError> {
    Ok(u32::from_le_bytes(read_header_bytes(content, 14, "4-byte payload length")?) as usize)
}

fn payload_slice(content: &[u8], payload_len: usize) -> Result<&[u8], FerraError> {
    let available = content.len() - HEADER_SIZE;
    if available < payload_len {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("{payload_len} bytes payload"),
            actual: format!("{available} bytes available"),
        });
    }
    Ok(&content[HEADER_SIZE..HEADER_SIZE + payload_len])
}

fn read_header_bytes<const N: usize>(
    content: &[u8],
    start: usize,
    expected: &str,
) -> Result<[u8; N], FerraError> {
    content[start..start + N]
        .try_into()
        .map_err(|_| FerraError::CheckpointCorrupted {
            expected: expected.to_string(),
            actual: "truncated".to_string(),
        })
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
