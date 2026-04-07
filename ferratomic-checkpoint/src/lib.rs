//! Checkpoint: serialize/deserialize datom stores with BLAKE3 integrity.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` — round-trip identity.
//! The datom set, indexes, schema, and epoch are preserved exactly
//! through serialization and deserialization. No datom is lost, added,
//! or reordered.
//!
//! ## Design: raw data, no Store dependency
//!
//! Serialize functions accept raw components (datoms, schema, epoch, etc.)
//! and return bytes. Deserialize functions return [`CheckpointData`] —
//! a plain struct of raw components. The Store module (in ferratomic-db)
//! reconstructs `Store` from `CheckpointData`. This avoids a circular
//! dependency between ferratomic-checkpoint and ferratomic-db.
//!
//! ## Format dispatch
//!
//! Deserialization dispatches on the first 4 magic bytes:
//! - `b"CHKP"` — V2 (legacy)
//! - `b"CHK3"` — V3 (pre-sorted, LIVE bitvector persisted)
//! - `b"CHK4"` — V4 (columnar, per-column bincode)
//!
//! Serialization produces V3 by default. V4 serialization is available via
//! [`serialize_v4_checkpoint_bytes`]. V2 read support is retained for
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

// INV-FERR-023: No unsafe code in ferratomic-checkpoint except mmap.rs (ADR-FERR-020).
// deny (not forbid) allows the single mmap module to #![allow(unsafe_code)].
#![deny(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

use std::{
    fs::File,
    io::{BufWriter, Write as IoWrite},
    path::Path,
};

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{AgentId, AttributeDef, Datom, FerraError};

pub mod v3;
pub mod v4;

pub mod mmap;

#[cfg(test)]
mod tests;

/// V2 checkpoint file magic bytes: ASCII "CHKP".
const CHECKPOINT_MAGIC: [u8; 4] = *b"CHKP";

/// V3 checkpoint file magic bytes: ASCII "CHK3".
/// Single source of truth — `v3.rs` imports this via `super::V3_MAGIC`.
const V3_MAGIC: [u8; 4] = *b"CHK3";

/// V4 checkpoint file magic bytes: ASCII "CHK4".
/// Single source of truth — `v4.rs` imports this via `super::V4_MAGIC`.
const V4_MAGIC: [u8; 4] = *b"CHK4";

/// Checkpoint format version. Little-endian u16. V2: u64 length field.
const CHECKPOINT_VERSION: u16 = 2;

/// Fixed header size: magic(4) + version(2) + epoch(8) + length(8) = 22 bytes.
const HEADER_SIZE: usize = 22;

/// BLAKE3 hash size: 32 bytes. Canonical definition in `mmap.rs`.
const HASH_SIZE: usize = crate::mmap::HASH_SIZE;

/// Minimum checkpoint file size: header + hash (empty payload).
const MIN_FILE_SIZE: usize = HEADER_SIZE + HASH_SIZE;

// ---------------------------------------------------------------------------
// CheckpointData — the raw data contract between checkpoint and store
// ---------------------------------------------------------------------------

/// Raw checkpoint data — the contract between checkpoint and store crates.
///
/// Used as both input (serialization) and output (deserialization).
/// The Store module in ferratomic-db reconstructs a Store from this struct.
/// This design avoids a circular dependency between checkpoint and store crates.
///
/// INV-FERR-013: All fields are preserved exactly through round-trip.
#[derive(Debug, Clone)]
pub struct CheckpointData {
    /// Store epoch at checkpoint time.
    pub epoch: u64,
    /// The genesis agent identity for Store reconstruction.
    pub genesis_agent: AgentId,
    /// Schema attributes as sorted (name, definition) pairs.
    pub schema_pairs: Vec<(String, AttributeDef)>,
    /// All datoms (either EAVT-sorted for V3, or unsorted for V2).
    pub datoms: Vec<Datom>,
    /// LIVE bitvector (INV-FERR-029): `live_bits[p] = true` iff datom p is live.
    /// `None` for V2 checkpoints (must be recomputed by caller).
    pub live_bits: Option<BitVec<u64, Lsb0>>,
}

impl CheckpointData {
    /// Borrow the LIVE bitvector, or return a `CheckpointWrite` error if absent.
    ///
    /// V3 serialization requires a LIVE bitvector. V2 deserialization produces
    /// `None` because V2 format did not persist liveness. This helper provides
    /// a clean error path for serialization functions that require it.
    fn require_live_bits(&self) -> Result<&BitVec<u64, Lsb0>, FerraError> {
        self.live_bits.as_ref().ok_or_else(|| {
            FerraError::CheckpointWrite(
                "live_bits required for V3 serialization but absent (V2 data?)".to_string(),
            )
        })
    }
}

/// Raw checkpoint data for LIVE-first partial loads (INV-FERR-075).
///
/// Contains LIVE datoms separately from historical datoms so the Store
/// module can build a LIVE-only store for fast cold start.
#[derive(Debug, Clone)]
pub struct PartialCheckpointData {
    /// Store epoch at checkpoint time.
    pub epoch: u64,
    /// The genesis agent identity.
    pub genesis_agent: AgentId,
    /// Schema attributes as sorted (name, definition) pairs.
    pub schema_pairs: Vec<(String, AttributeDef)>,
    /// LIVE datoms in canonical EAVT order (INV-FERR-029).
    pub live_datoms: Vec<Datom>,
    /// Historical (non-LIVE) datoms in canonical EAVT order.
    pub hist_datoms: Vec<Datom>,
}

// ---------------------------------------------------------------------------
// Serialize
// ---------------------------------------------------------------------------

/// Serialize checkpoint data to bytes (in-memory) using V3 format.
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// schema, genesis agent, all datoms, LIVE bitvector) in the V3 checkpoint
/// wire format. A trailing BLAKE3 hash covers all preceding bytes for
/// tamper detection.
///
/// **Performance: O(n) where n is the total datom count in the store.**
/// INV-FERR-013 (round-trip identity) requires the checkpoint to capture
/// the complete datom set; any omission would violate `load(checkpoint(S)) = S`.
/// The BLAKE3 hash is also O(n) over the serialized bytes. Phase 4b
/// optimization path: incremental/delta checkpoints that serialize only
/// datoms added since the previous checkpoint, with a merge-on-load strategy.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails or if
/// `live_bits` is `None` (required for V3 format).
pub fn serialize_checkpoint_bytes(data: &CheckpointData) -> Result<Vec<u8>, FerraError> {
    let live_bits = data.require_live_bits()?;
    v3::serialize_v3_bytes(
        &data.datoms,
        &data.schema_pairs,
        data.epoch,
        data.genesis_agent,
        live_bits,
    )
}

/// Serialize checkpoint data to LIVE-first V3 checkpoint bytes (INV-FERR-075).
///
/// LIVE datoms are stored first, historical datoms second. Version field
/// 0x0103 distinguishes from standard V3.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails or if
/// `live_bits` is `None`.
pub fn serialize_live_first_bytes(data: &CheckpointData) -> Result<Vec<u8>, FerraError> {
    let live_bits = data.require_live_bits()?;
    v3::serialize_v3_live_first(
        &data.datoms,
        &data.schema_pairs,
        data.epoch,
        data.genesis_agent,
        live_bits,
    )
}

/// Serialize checkpoint data to V4 columnar checkpoint bytes.
///
/// V4 decomposes each datom into per-column arrays (entity, attribute,
/// value, tx, op) and serializes them independently. This layout enables
/// per-column compression in Phase 4b. Phase 4a uses bincode for all columns.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails or if
/// `live_bits` is `None`.
pub fn serialize_v4_checkpoint_bytes(data: &CheckpointData) -> Result<Vec<u8>, FerraError> {
    let live_bits = data.require_live_bits()?;
    v4::serialize_v4_bytes(
        &data.datoms,
        &data.schema_pairs,
        data.epoch,
        data.genesis_agent,
        live_bits,
    )
}

// ---------------------------------------------------------------------------
// Deserialize
// ---------------------------------------------------------------------------

/// Deserialize checkpoint bytes (in-memory), dispatching on magic bytes.
///
/// INV-FERR-013: Examines the first 4 bytes to determine the format:
/// - `b"CHKP"` -> V2 (legacy)
/// - `b"CHK3"` -> V3 (pre-sorted, LIVE bitvector persisted)
/// - `b"CHK4"` -> V4 (columnar, per-column bincode)
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on unknown magic, checksum
/// mismatch, format errors, or deserialization failure.
pub fn deserialize_checkpoint_bytes(data: &[u8]) -> Result<CheckpointData, FerraError> {
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
        V4_MAGIC => v4::deserialize_v4_bytes(data),
        _ => Err(FerraError::CheckpointCorrupted {
            expected: "CHKP, CHK3, or CHK4".to_string(),
            actual: String::from_utf8_lossy(&magic).to_string(),
        }),
    }
}

/// Deserialize a LIVE-first V3 checkpoint into partial data (INV-FERR-075).
///
/// Returns a [`PartialCheckpointData`] with LIVE datoms separate from
/// historical datoms for lazy merge.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on integrity or format errors.
pub fn deserialize_live_first_partial(data: &[u8]) -> Result<PartialCheckpointData, FerraError> {
    v3::deserialize_v3_live_first_partial(data)
}

// ---------------------------------------------------------------------------
// V2 deserialization (legacy read support)
// ---------------------------------------------------------------------------

/// Deserialize from V2 checkpoint bytes (in-memory).
///
/// INV-FERR-013: Legacy V2 format. Verifies the BLAKE3 checksum before
/// reconstructing checkpoint data.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// format errors, or deserialization failure.
fn deserialize_v2_bytes(data: &[u8]) -> Result<CheckpointData, FerraError> {
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
    let datoms: Vec<Datom> = wire_datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();

    Ok(CheckpointData {
        epoch,
        genesis_agent,
        schema_pairs: schema,
        datoms,
        live_bits: None, // V2 has no LIVE bitvector; caller must recompute.
    })
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Write checkpoint data to a file (V3 format).
///
/// INV-FERR-013: The checkpoint contains the full store state in a format
/// that `load_checkpoint` can reconstruct exactly. A trailing BLAKE3 hash
/// covers all preceding bytes for tamper detection.
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
pub fn write_checkpoint(data: &CheckpointData, path: &Path) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(data)?;

    // HI-001: Atomic write via temp file + rename. A crash between
    // temp creation and rename leaves the original checkpoint intact.
    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(format!(".checkpoint.{}.tmp", std::process::id()));

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
pub fn write_checkpoint_live_first(data: &CheckpointData, path: &Path) -> Result<(), FerraError> {
    let buf = serialize_live_first_bytes(data)?;

    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(format!(".checkpoint_lf.{}.tmp", std::process::id()));

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

/// Load checkpoint data from a file.
///
/// INV-FERR-013: Verifies the BLAKE3 checksum before reconstructing
/// checkpoint data. Returns an error if the file is truncated, the magic
/// is wrong, or the checksum fails.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// `FerraError::Io` on read failure, or `FerraError::CheckpointWrite`
/// on deserialization failure.
pub fn load_checkpoint(path: &Path) -> Result<CheckpointData, FerraError> {
    let data = std::fs::read(path).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;

    deserialize_checkpoint_bytes(&data)
}

/// Load checkpoint data from an arbitrary reader (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint loading for `StorageBackend` implementations.
///
/// # Errors
///
/// Returns `FerraError::Io` on read failure or `FerraError::CheckpointCorrupted`
/// on checksum/format errors.
pub fn load_checkpoint_from_reader<R: std::io::Read>(
    reader: &mut R,
) -> Result<CheckpointData, FerraError> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;
    deserialize_checkpoint_bytes(&data)
}

/// Write checkpoint data to an arbitrary writer (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint writing for `StorageBackend` implementations.
///
/// INV-FERR-013: `load(checkpoint(S)) = S` -- round-trip identity.
/// INV-FERR-024: substrate agnosticism -- writes through any `std::io::Write`
/// implementor, decoupling the checkpoint protocol from filesystem specifics.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization or the write fails.
pub fn write_checkpoint_to_writer<W: std::io::Write>(
    data: &CheckpointData,
    writer: &mut W,
) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(data)?;
    writer
        .write_all(&buf)
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    writer
        .flush()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

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
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
