//! Checkpoint: serialize Store to a durable file with BLAKE3 integrity.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` — round-trip identity.
//! The datom set, indexes, schema, and epoch are preserved exactly
//! through serialization and deserialization. No datom is lost, added,
//! or reordered.
//!
//! ## Format dispatch
//!
//! Deserialization dispatches on the first 4 magic bytes:
//! - `b"CHKP"` — V2 (legacy)
//! - `b"CHK3"` — V3 (pre-sorted, LIVE bitvector persisted)
//!
//! Serialization always produces V3. V2 read support is retained for
//! backward compatibility with existing checkpoint files.
//!
//! ## V2 File Format (legacy)
//!
//! ```text
//! +------------------+
//! | Magic    (4B)    | 0x43484B50 ("CHKP")
//! +------------------+
//! | Version  (2B)    | 0x0002 (little-endian, V2)
//! +------------------+
//! | Epoch    (8B)    | u64 little-endian
//! +------------------+
//! | Length   (8B)    | u64 byte count of bincode payload
//! +------------------+
//! | Payload  (N)     | bincode: { schema, genesis_agent, datoms }
//! +------------------+
//! | BLAKE3   (32B)   | Hash of all preceding bytes
//! +------------------+
//! ```
//!
//! ## V3 File Format
//!
//! See [`v3`] module documentation for the V3 wire format.
//!
//! CR-005: The payload uses bincode serialization (matching the WAL format)
//! for INV-FERR-028 compliance. At 100M datoms, bincode produces ~20GB
//! (parseable in <5s on `NVMe`). Per ADR-FERR-010, deserialization goes through
//! wire types (`WireCheckpointPayload` / `V3PayloadRead`) for trust boundary
//! enforcement.

#[cfg(test)]
use std::collections::BTreeMap;
use std::{
    fs::File,
    io::{BufWriter, Write as IoWrite},
    path::Path,
};

use ferratom::FerraError;
#[cfg(test)]
use ferratom::{AgentId, AttributeDef, Datom};
#[cfg(test)]
use serde::Serialize;

use crate::store::Store;

pub(crate) mod v3;

#[cfg(test)]
mod tests;

/// V2 checkpoint file magic bytes: ASCII "CHKP".
const CHECKPOINT_MAGIC: [u8; 4] = *b"CHKP";

/// V3 checkpoint file magic bytes: ASCII "CHK3".
const V3_MAGIC: [u8; 4] = *b"CHK3";

/// Checkpoint format version. Little-endian u16. V2: u64 length field.
const CHECKPOINT_VERSION: u16 = 2;

/// Fixed header size: magic(4) + version(2) + epoch(8) + length(8) = 22 bytes.
const HEADER_SIZE: usize = 22;

/// BLAKE3 hash size: 32 bytes.
const HASH_SIZE: usize = 32;

/// Minimum checkpoint file size: header + hash (empty payload).
const MIN_FILE_SIZE: usize = HEADER_SIZE + HASH_SIZE;

/// V2 checkpoint payload (serialization only, test use).
///
/// ADR-FERR-010: Deserialization uses `WireCheckpointPayload` from the
/// `ferratom::wire` module instead. This struct retains `Serialize` only.
/// Schema attributes are sorted by name for deterministic output.
/// Datoms are in `OrdSet` iteration order (`Datom`'s `Ord` impl = EAVT).
#[cfg(test)]
#[derive(Serialize)]
struct CheckpointPayload {
    /// Schema attributes as sorted (name, definition) pairs.
    schema: Vec<(String, AttributeDef)>,
    /// The genesis agent identity for Store reconstruction.
    genesis_agent: AgentId,
    /// All datoms in deterministic EAVT order.
    datoms: Vec<Datom>,
}

/// Build and serialize the V2 checkpoint payload (schema + agent + datoms).
///
/// INV-FERR-013: schema is sorted by attribute name for determinism.
/// Datoms are in `OrdSet` iteration order (`Datom::Ord` = EAVT).
/// CR-005: bincode for INV-FERR-028 cold-start compliance.
#[cfg(test)]
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

/// Serialize a store to checkpoint bytes (in-memory) using V3 format.
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// schema, genesis agent, all datoms, LIVE bitvector) in the V3 checkpoint
/// wire format. A trailing BLAKE3 hash covers all preceding bytes for
/// tamper detection. `deserialize_checkpoint_bytes` can reconstruct the
/// store exactly.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub(crate) fn serialize_checkpoint_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    v3::serialize_v3_bytes(store)
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
    v3::serialize_v3_live_first(store)
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
pub fn deserialize_live_first_partial(data: &[u8]) -> Result<v3::PartialStore, FerraError> {
    v3::deserialize_v3_live_first_partial(data)
}

/// Re-export `PartialStore` for public API consumers (INV-FERR-075).
pub use v3::PartialStore;

/// Serialize a store to V2 checkpoint bytes (in-memory).
///
/// INV-FERR-013: Legacy V2 format. Used in tests for V2/V3 equivalence
/// verification.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if the payload exceeds
/// `u32::MAX` bytes or serialization fails.
#[cfg(test)]
fn serialize_v2_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let epoch = store.epoch();
    let payload_bytes = build_payload_bytes(store)?;

    // V2: u64 length field supports payloads > 4GB (INV-FERR-028: 100M datoms).
    // usize→u64 is lossless on all Rust-supported platforms (min 32-bit).
    let payload_len = payload_bytes.len() as u64;

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
    if data.len() < 4 {
        return Err(FerraError::CheckpointCorrupted {
            expected: "at least 4 bytes for magic".to_string(),
            actual: format!("{} bytes", data.len()),
        });
    }

    let magic: [u8; 4] = data[0..4]
        .try_into()
        .map_err(|_| FerraError::CheckpointCorrupted {
            expected: "4-byte magic".to_string(),
            actual: "truncated".to_string(),
        })?;

    match magic {
        CHECKPOINT_MAGIC => deserialize_v2_bytes(data),
        V3_MAGIC => {
            // V3 family: dispatch by version field.
            // 0x0003 = standard V3, 0x0103 = LIVE-first V3 (INV-FERR-075).
            if data.len() < 6 {
                return Err(FerraError::CheckpointCorrupted {
                    expected: "at least 6 bytes for version".to_string(),
                    actual: format!("{} bytes", data.len()),
                });
            }
            let version = u16::from_le_bytes([data[4], data[5]]);
            match version {
                3 => v3::deserialize_v3_bytes(data),
                v3::V3_LIVE_FIRST_VERSION => v3::deserialize_v3_live_first_full(data),
                _ => Err(FerraError::CheckpointCorrupted {
                    expected: "V3 version 0x0003 or 0x0103".to_string(),
                    actual: format!("version {version:#06x}"),
                }),
            }
        }
        _ => Err(FerraError::CheckpointCorrupted {
            expected: "CHKP or CHK3".to_string(),
            actual: String::from_utf8_lossy(&magic).to_string(),
        }),
    }
}

/// Deserialize a store from V2 checkpoint bytes (in-memory).
///
/// INV-FERR-013: Legacy V2 format. Verifies the BLAKE3 checksum before
/// reconstructing the store.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// format errors, or deserialization failure.
pub(crate) fn deserialize_v2_bytes(data: &[u8]) -> Result<Store, FerraError> {
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

    // ADR-FERR-010: Decompose wire payload and convert through trust boundary.
    let (schema, genesis_agent, wire_datoms) = wire_payload.into_parts();
    let datoms: Vec<ferratom::Datom> = wire_datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();

    Ok(Store::from_checkpoint(epoch, genesis_agent, schema, datoms))
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
    let buf = serialize_live_first_bytes(store)?;

    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(".checkpoint_lf.tmp");

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

    std::fs::rename(&tmp_path, path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
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
    let data = std::fs::read(path).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;

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
    reader.read_to_end(&mut data).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;
    deserialize_checkpoint_bytes(&data)
}

/// Write a checkpoint to an arbitrary writer (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint writing for [`StorageBackend`](crate::storage::StorageBackend)
/// implementations. The checkpoint contains the full store state (epoch,
/// schema, genesis agent, all datoms, LIVE bitvector) in V3 format with
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
            expected: format!("version {CHECKPOINT_VERSION} (V2)"),
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
    let len = u64::from_le_bytes(read_header_bytes(content, 14, "8-byte payload length")?);
    usize::try_from(len).map_err(|_| FerraError::CheckpointCorrupted {
        expected: "payload length within usize range".to_string(),
        actual: format!("{len} bytes"),
    })
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
pub(super) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
